//! Modules: IMPORT, aliases, namespacing, once semantics and diagnostics.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use basic_vm::{Error, Vm, compile, compile_file};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A unique temporary directory removed on drop (only this directory).
struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "basic-vm-modules-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_path(path: &Path) -> (String, Result<(), Error>) {
    let program = match compile_file(path) {
        Ok(program) => program,
        Err(error) => return (String::new(), Err(error)),
    };
    let mut vm = Vm::new(program).unwrap();
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    (String::from_utf8(output).unwrap(), result)
}

/// Writes `main.bas` plus the given files and runs it.
fn run_files(main: &str, files: &[(&str, &str)]) -> (String, Result<(), Error>) {
    let dir = Dir::new();
    for (name, text) in files {
        dir.file(name, text);
    }
    let path = dir.file("main.bas", main);
    run_path(&path)
}

fn ok(main: &str, files: &[(&str, &str)]) -> String {
    let (output, result) = run_files(main, files);
    result.unwrap_or_else(|error| panic!("{error}"));
    output
}

fn failure(main: &str, files: &[(&str, &str)]) -> String {
    let (_, result) = run_files(main, files);
    result.expect_err("expected an error").to_string()
}

#[test]
fn functions_constants_and_globals_are_reached_through_the_alias() {
    let out = ok(
        "IMPORT \"m.bas\" AS M\nPRINT M.Twice(4); \" \"; M.K; \" \"; M.count\nM.count = 9\n\
         PRINT M.Count2()\n",
        &[(
            "m.bas",
            "CONST K = 7\ncount = 1\nFUNCTION Twice(PARAMETER n)\ncount = count + 1\nTwice = n * 2\nFEND\n\
             FUNCTION Count2\nCount2 = count\nFEND\n",
        )],
    );
    assert_eq!(out, "8 7 2\n9\n");
}

#[test]
fn default_alias_is_the_upper_cased_file_stem() {
    let out = ok(
        "IMPORT \"tools.bas\"\nPRINT TOOLS.One()\n",
        &[("tools.bas", "FUNCTION One\nOne = 1\nFEND\n")],
    );
    assert_eq!(out, "1\n");
}

#[test]
fn module_globals_are_private_and_functions_see_their_own_globals() {
    let out = ok(
        "x = 100\nIMPORT \"m.bas\" AS M\nPRINT M.Get(); \" \"; x\nCALL M.Set(5)\nPRINT M.Get(); \" \"; x\n",
        &[(
            "m.bas",
            "x = 1\nFUNCTION Get\nGet = x\nFEND\nFUNCTION Set(PARAMETER v)\nx = v\nFEND\n",
        )],
    );
    // The importer's `x` is untouched; the import ran before the importer's code.
    assert_eq!(out, "1 100\n5 100\n");
}

#[test]
fn a_module_cannot_see_the_importers_globals() {
    let message = failure(
        "secret = 5\nIMPORT \"m.bas\" AS M\nPRINT M.Peek()\n",
        &[("m.bas", "FUNCTION Peek\nPeek = secret\nFEND\n")],
    );
    assert!(message.contains("m.bas"), "{message}");
    assert!(message.contains("undefined variable M.SECRET"), "{message}");
    assert!(message.contains("M.PEEK"), "{message}");
}

#[test]
fn imports_run_once_in_order_before_the_importer() {
    let out = ok(
        "PRINT \"main\"\nIMPORT \"a.bas\"\nIMPORT \"c.bas\"\nPRINT \"end\"\n",
        &[
            ("a.bas", "IMPORT \"b.bas\"\nPRINT \"a\"\n"),
            ("b.bas", "PRINT \"b\"\n"),
            ("c.bas", "IMPORT \"b.bas\" AS Other\nPRINT \"c\"\n"),
        ],
    );
    assert_eq!(out, "b\na\nc\nmain\nend\n");
}

#[test]
fn importing_one_file_twice_shares_a_single_instance() {
    let out = ok(
        "IMPORT \"m.bas\" AS One\nIMPORT \"./m.bas\" AS Two\nCALL One.Bump()\nCALL Two.Bump()\n\
         PRINT One.Total(); \" \"; Two.Total()\n",
        &[(
            "m.bas",
            "PRINT \"loaded\"\nn = 0\nFUNCTION Bump\nn = n + 1\nFEND\nFUNCTION Total\nTotal = n\nFEND\n",
        )],
    );
    assert_eq!(out, "loaded\n2 2\n");
}

#[test]
fn cyclic_imports_are_an_error_naming_the_chain() {
    let message = failure(
        "IMPORT \"a.bas\"\n",
        &[
            ("a.bas", "IMPORT \"b.bas\"\n"),
            ("b.bas", "REM x\nIMPORT \"a.bas\"\n"),
        ],
    );
    assert!(message.contains("import cycle"), "{message}");
    assert!(message.contains("a.bas"), "{message}");
    assert!(message.contains("b.bas"), "{message}");
    assert!(message.contains("->"), "{message}");
    assert!(message.contains("b.bas:2:"), "{message}");
    let message = failure("IMPORT \"main.bas\"\n", &[]);
    assert!(message.contains("import cycle"), "{message}");
}

#[test]
fn missing_and_unreadable_files_are_reported_at_the_import_line() {
    let message = failure("PRINT 1\nIMPORT \"nope.bas\"\n", &[]);
    assert!(message.starts_with("line 2:"), "{message}");
    assert!(message.contains("cannot import \"nope.bas\""), "{message}");
    let message = failure(
        "IMPORT \"a.bas\"\n",
        &[("a.bas", "\n\nIMPORT \"gone.bas\"\n")],
    );
    assert!(message.contains("a.bas:3:"), "{message}");
    assert!(message.contains("cannot import \"gone.bas\""), "{message}");
}

#[test]
fn invalid_utf8_in_an_imported_file_is_an_error() {
    let dir = Dir::new();
    fs::write(dir.0.join("bad.bas"), [0x50, 0xFF, 0xFE, 0x0A]).unwrap();
    let main = dir.file("main.bas", "IMPORT \"bad.bas\"\n");
    let (_, result) = run_path(&main);
    let message = result.unwrap_err().to_string();
    assert!(message.contains("cannot import \"bad.bas\""), "{message}");
    assert!(message.starts_with("line 1:"), "{message}");
}

#[test]
fn compile_errors_in_imported_files_report_path_and_line() {
    let message = failure("IMPORT \"m.bas\"\n", &[("m.bas", "PRINT 1\nPRINT 2 +\n")]);
    assert!(message.contains("m.bas:2:"), "{message}");
    // The main file keeps the legacy format.
    let message = failure("PRINT 1\nPRINT 2 +\n", &[]);
    assert!(message.starts_with("line 2:"), "{message}");
}

#[test]
fn runtime_errors_in_imported_functions_report_path_line_and_qualified_name() {
    let (_, result) = run_files(
        "IMPORT \"m.bas\" AS Lib\nPRINT Lib.Boom()\n",
        &[("m.bas", "FUNCTION Boom\n\nBoom = 1 / 0\nFEND\n")],
    );
    let error = result.unwrap_err();
    assert_eq!(error.line, 3);
    assert_eq!(error.function.as_deref(), Some("LIB.BOOM"));
    assert!(error.file.as_deref().unwrap().ends_with("m.bas"));
    let text = error.to_string();
    assert!(text.contains("m.bas:3:"), "{text}");
    assert!(text.contains("(in function LIB.BOOM)"), "{text}");
}

#[test]
fn runtime_errors_in_main_after_imports_keep_the_legacy_format() {
    let (_, result) = run_files("IMPORT \"m.bas\"\nX = 1 / 0\n", &[("m.bas", "Y = 1\n")]);
    let error = result.unwrap_err();
    assert_eq!(error.file, None);
    assert!(error.to_string().starts_with("line 2:"), "{error}");
}

#[test]
fn caught_errors_from_modules_still_work() {
    let out = ok(
        "IMPORT \"m.bas\" AS M\nTRY\nPRINT M.Boom()\nCATCH E$\nPRINT \"caught \"; E$; \" \"; ERRFUNC$()\nEND TRY\n",
        &[("m.bas", "FUNCTION Boom\nTHROW \"bad\"\nFEND\n")],
    );
    assert_eq!(out, "caught bad M.BOOM\n");
}

#[test]
fn import_must_be_at_top_level() {
    let message = failure(
        "FUNCTION F\nIMPORT \"m.bas\"\nFEND\n",
        &[("m.bas", "X = 1\n")],
    );
    assert!(
        message.contains("IMPORT must be at the top level"),
        "{message}"
    );
    let message = failure(
        "IF 1 THEN\nIMPORT \"m.bas\"\nEND IF\n",
        &[("m.bas", "X = 1\n")],
    );
    assert!(
        message.contains("IMPORT must be at the top level"),
        "{message}"
    );
}

#[test]
fn qualified_record_types() {
    let out = ok(
        "IMPORT \"shapes.bas\" AS S\nDIM p AS S.Pt = NEW S.Pt(x = 1, y = 2)\nPRINT p.x; p.y\n\
         DIM q AS S.Pt = S.Make(5)\nPRINT q.x; q.y\nPRINT TYPENAME$(q)\n\
         CALL S.Move(p)\nPRINT p.x\n",
        &[(
            "shapes.bas",
            "TYPE Pt\n x AS INTEGER\n y AS INTEGER\nEND TYPE\n\
             FUNCTION Make(PARAMETER n) AS Pt\nMake = NEW Pt(x = n, y = n)\nFEND\n\
             FUNCTION Move(PARAMETER p AS Pt)\np.x = p.x + 10\nFEND\n",
        )],
    );
    assert_eq!(out, "12\n55\nS.PT\n11\n");
}

#[test]
fn unknown_members_and_constant_assignment_are_compile_errors() {
    let files = [("m.bas", "CONST K = 1\nv = 2\nFUNCTION F\nFEND\n")];
    let message = failure("IMPORT \"m.bas\" AS M\nPRINT M.Nope\n", &files);
    assert!(
        message.contains("module M has no variable or constant NOPE"),
        "{message}"
    );
    let message = failure("IMPORT \"m.bas\" AS M\nPRINT M.Nope()\n", &files);
    assert!(
        message.contains("module M has no function NOPE"),
        "{message}"
    );
    let message = failure("IMPORT \"m.bas\" AS M\nM.K = 5\n", &files);
    assert!(message.contains("cannot assign to constant"), "{message}");
    let message = failure("IMPORT \"m.bas\" AS M\nDIM p AS M.Missing\n", &files);
    assert!(message.contains("unknown type"), "{message}");
}

#[test]
fn alias_resolution_shadows_a_record_variable_of_the_same_name() {
    // `M.x` means the module member when M is an alias; documented rule.
    let out = ok(
        "IMPORT \"m.bas\" AS M\nPRINT M.x\n",
        &[("m.bas", "x = 3\n")],
    );
    assert_eq!(out, "3\n");
}

#[test]
fn legacy_dotted_field_access_still_works_without_aliases() {
    let out = ok(
        "IMPORT \"m.bas\" AS M\nTYPE T\n a AS INTEGER\nEND TYPE\nDIM r AS T\nr.a = 4\nPRINT r.a\n",
        &[("m.bas", "x = 3\n")],
    );
    assert_eq!(out, "4\n");
}

#[test]
fn option_explicit_applies_per_module() {
    // Module declares OPTION EXPLICIT; the importer does not need to.
    let files = [(
        "m.bas",
        "OPTION EXPLICIT\nDIM a AS INTEGER = 1\nFUNCTION F\nF = a\nFEND\n",
    )];
    assert_eq!(
        ok(
            "IMPORT \"m.bas\" AS M\nundeclared = 2\nPRINT M.F(); undeclared\n",
            &files
        ),
        "12\n"
    );
    // An implicit variable inside the explicit module is an error in that file.
    let message = failure(
        "IMPORT \"m.bas\" AS M\n",
        &[("m.bas", "OPTION EXPLICIT\nb = 1\n")],
    );
    assert!(message.contains("m.bas:2:"), "{message}");
    // The importer's OPTION EXPLICIT does not leak into modules.
    let out = ok(
        "OPTION EXPLICIT\nIMPORT \"m.bas\" AS M\nPRINT M.F()\n",
        &[("m.bas", "free = 1\nFUNCTION F\nF = free\nFEND\n")],
    );
    assert_eq!(out, "1\n");
}

#[test]
fn line_numbers_and_constants_are_scoped_per_module() {
    let out = ok(
        "CONST K = 1\nIMPORT \"m.bas\" AS M\n10 PRINT K; M.K\n",
        &[(
            "m.bas",
            "CONST K = 2\n10 PRINT \"m10\"\nGOTO 20\n20 PRINT K\n",
        )],
    );
    assert_eq!(out, "m10\n2\n12\n");
}

#[test]
fn functions_with_the_same_name_in_different_modules_do_not_clash() {
    let out = ok(
        "IMPORT \"a.bas\" AS A\nIMPORT \"b.bas\" AS B\nPRINT A.Name$(); B.Name$(); Name$()\n\
         FUNCTION Name$\nName$ = \"main\"\nFEND\n",
        &[
            ("a.bas", "FUNCTION Name$\nName$ = \"a \"\nFEND\n"),
            ("b.bas", "FUNCTION Name$\nName$ = \"b \"\nFEND\n"),
        ],
    );
    assert_eq!(out, "a b main\n");
}

#[test]
fn modules_call_each_others_functions_and_imported_names_via_their_own_aliases() {
    let out = ok(
        "IMPORT \"a.bas\" AS A\nPRINT A.Double(21)\n",
        &[
            (
                "a.bas",
                "IMPORT \"sub/b.bas\" AS Helper\nFUNCTION Double(PARAMETER n)\nDouble = Helper.Add(n, n)\nFEND\n",
            ),
            (
                "sub/b.bas",
                "FUNCTION Add(PARAMETER a, PARAMETER b = 0)\nAdd = a + b\nFEND\n",
            ),
        ],
    );
    assert_eq!(out, "42\n");
}

#[test]
fn module_function_signatures_are_checked_at_the_call_site() {
    let files = [("m.bas", "FUNCTION F(PARAMETER a, PARAMETER b = 1)\nFEND\n")];
    let message = failure("IMPORT \"m.bas\" AS M\nCALL M.F(c := 1)\n", &files);
    assert!(
        message.contains("function M.F has no parameter C"),
        "{message}"
    );
    let message = failure("IMPORT \"m.bas\" AS M\nCALL M.F()\n", &files);
    assert!(
        message.contains("function M.F requires 1 to 2 arguments, got 0"),
        "{message}"
    );
}

#[test]
fn byref_works_on_module_globals() {
    let out = ok(
        "IMPORT \"m.bas\" AS M\nCALL Inc(M.n)\nPRINT M.n\nFUNCTION Inc(PARAMETER BYREF v)\nv = v + 1\nFEND\n",
        &[("m.bas", "n = 41\n")],
    );
    assert_eq!(out, "42\n");
}

#[test]
fn string_sources_resolve_imports_from_the_current_directory_or_absolute_paths() {
    let dir = Dir::new();
    let module = dir.file("abs.bas", "FUNCTION One\nOne = 1\nFEND\n");
    let source = format!(
        "IMPORT \"{}\" AS Abs\nPRINT Abs.One()\n",
        module.display().to_string().replace('\\', "/")
    );
    let program = compile(&source).unwrap();
    let mut vm = Vm::new(program).unwrap();
    let mut output = Vec::new();
    vm.run(&mut Cursor::new(""), &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "1\n");
    let error = compile("IMPORT \"definitely_missing_module.bas\"\n").unwrap_err();
    assert!(error.to_string().contains("cannot import"), "{error}");
}

#[test]
fn cli_resolves_imports_relative_to_the_importing_file_not_the_cwd() {
    let dir = Dir::new();
    dir.file(
        "lib/util.bas",
        "IMPORT \"deep/inner.bas\" AS Inner\nFUNCTION Up(PARAMETER n)\nUp = Inner.Inc(n)\nFEND\n",
    );
    dir.file(
        "lib/deep/inner.bas",
        "FUNCTION Inc(PARAMETER n)\nInc = n + 1\nFEND\n",
    );
    let main = dir.file(
        "app/main.bas",
        "IMPORT \"../lib/util.bas\" AS U\nPRINT U.Up(1)\n",
    );
    let elsewhere = Dir::new();
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .current_dir(&elsewhere.0)
        .arg(&main)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "2\n");
}

#[test]
fn cli_reports_errors_in_imported_files_with_path_and_line() {
    let dir = Dir::new();
    dir.file("m.bas", "PRINT 1\nX = 1 / 0\n");
    let main = dir.file("main.bas", "IMPORT \"m.bas\"\n");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&main)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("m.bas:2:"), "{stderr}");
}
