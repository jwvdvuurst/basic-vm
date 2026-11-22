use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::process::ExitCode;

use basic_vm::{
    AutoClose, ConsoleKeys, GraphicsBackend, HeadlessBackend, ImageFormat, TestOptions, Vm,
    compile_from, enable_terminal_sequences, run_tests_with,
};

/// Stands in for the window backend in builds without the `window` feature.
#[cfg(not(feature = "window"))]
struct UnavailableBackend;

#[cfg(not(feature = "window"))]
impl GraphicsBackend for UnavailableBackend {
    fn open(&mut self, _: usize, _: usize, _: &str) -> Result<(), String> {
        Err(
            "this build has no window backend (rebuilt without the `window` feature); \
             use --headless-graphics"
                .into(),
        )
    }
    fn close(&mut self) {}
    fn present(&mut self, _: &basic_vm::Canvas) -> Result<(), String> {
        Ok(())
    }
    fn poll_events(&mut self) -> Vec<basic_vm::Event> {
        Vec::new()
    }
    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

fn window_backend() -> Box<dyn GraphicsBackend> {
    #[cfg(feature = "window")]
    {
        Box::new(basic_vm::WindowBackend::new())
    }
    #[cfg(not(feature = "window"))]
    {
        Box::new(UnavailableBackend)
    }
}

fn number_after(
    args: &mut std::iter::Peekable<impl Iterator<Item = std::ffi::OsString>>,
    flag: &str,
) -> Result<u64, String> {
    args.next();
    args.peek()
        .and_then(|value| value.to_str())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| format!("expected a non-negative whole number after {flag}"))
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("basic-vm: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Exit status for command-line misuse of the test mode.
const USAGE_ERROR: u8 = 2;

fn usage_error(message: &str) -> ExitCode {
    eprintln!("basic-vm: {message}");
    ExitCode::from(USAGE_ERROR)
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let usage = "usage: basic-vm [--dump-bytecode] [--headless-graphics] [--auto-close-ms <n>] [--max-frames <n>] [--screenshot <file.ppm|file.bmp|file.png>] <program.bas> [program arguments...]\n       basic-vm --test [--test-filter <text>] [--test-imports] <program.bas>";
    let mut args = env::args_os().skip(1).peekable();
    let mut dump = false;
    let mut headless = false;
    let mut screenshot: Option<std::path::PathBuf> = None;
    let mut auto_close_ms: Option<u64> = None;
    let mut max_frames: Option<u64> = None;
    let mut test_mode = false;
    let mut test_filter: Option<String> = None;
    let mut test_imports = false;
    // Flags are only recognised before the program path.
    while let Some(flag) = args.peek() {
        if flag == "--dump-bytecode" {
            dump = true;
        } else if flag == "--headless-graphics" {
            headless = true;
        } else if flag == "--test" || flag == "--run-tests" {
            test_mode = true;
        } else if flag == "--test-imports" {
            test_imports = true;
        } else if flag == "--test-filter" {
            args.next();
            match args.peek().and_then(|text| text.to_str()) {
                Some(text) => test_filter = Some(text.to_string()),
                None => return Ok(usage_error("expected text after --test-filter")),
            }
        } else if flag == "--auto-close-ms" {
            auto_close_ms = Some(number_after(&mut args, "--auto-close-ms")?);
        } else if flag == "--max-frames" {
            max_frames = Some(number_after(&mut args, "--max-frames")?);
        } else if flag == "--screenshot" {
            args.next();
            let file = args
                .peek()
                .ok_or("expected a .ppm, .bmp or .png file name after --screenshot")?;
            screenshot = Some(file.into());
            headless = true;
        } else if flag.to_str().is_some_and(|text| text.starts_with("--")) {
            return Err(format!("unknown option {}", flag.to_string_lossy()).into());
        } else {
            break;
        }
        args.next();
    }
    if test_mode {
        let conflicting = dump
            || screenshot.is_some()
            || auto_close_ms.is_some()
            || max_frames.is_some()
            || headless;
        if conflicting {
            return Ok(usage_error(
                "--test cannot be combined with --dump-bytecode, --screenshot, --auto-close-ms, --max-frames or --headless-graphics (tests always use the headless backend)",
            ));
        }
        let Some(path) = args.next() else {
            return Ok(usage_error(usage));
        };
        if args.next().is_some() {
            return Ok(usage_error("--test takes no program arguments"));
        }
        return run_test_mode(&path, test_filter, test_imports);
    }
    if test_filter.is_some() || test_imports {
        return Ok(usage_error(
            "--test-filter and --test-imports require --test",
        ));
    }
    let path = args.next().ok_or(usage)?;
    // Everything after the program path is passed to ARGC()/ARG$(n).
    let program_arguments = args
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "command-line arguments must be valid Unicode")
        })
        .collect::<Result<Vec<_>, _>>()?;
    if dump && !program_arguments.is_empty() {
        return Err(usage.into());
    }
    if let Some(file) = &screenshot
        && ImageFormat::from_path(file).is_none()
    {
        return Err("--screenshot file must end in .ppm, .bmp or .png".into());
    }
    let source = fs::read_to_string(&path)?;
    let program = compile_from(&source, std::path::Path::new(&path))?;
    if dump {
        let mut output = io::stdout().lock();
        use std::io::Write;
        for (index, instruction) in program.instructions().iter().enumerate() {
            if instruction.file == 0 {
                writeln!(
                    output,
                    "{index:04} [line {}] {:?}",
                    instruction.line, instruction.op
                )?;
            } else {
                writeln!(
                    output,
                    "{index:04} [{}:{}] {:?}",
                    program.file_name(instruction.file),
                    instruction.line,
                    instruction.op
                )?;
            }
        }
        output.flush()?;
    } else {
        let mut vm = Vm::new(program)?;
        vm.set_arguments(program_arguments);
        if io::stdout().is_terminal() {
            enable_terminal_sequences();
        }
        vm.set_key_provider(Box::new(ConsoleKeys::new()));
        let backend = HeadlessBackend::new();
        let inner: Box<dyn GraphicsBackend> = if headless {
            Box::new(backend.clone())
        } else {
            window_backend()
        };
        if auto_close_ms.is_some() || max_frames.is_some() {
            vm.set_graphics_backend(Box::new(AutoClose::new(inner, auto_close_ms, max_frames)));
        } else {
            vm.set_graphics_backend(inner);
        }
        let result = vm.run(&mut io::stdin().lock(), &mut io::stdout().lock());
        let saved = match &screenshot {
            Some(file) => save_screenshot(file, &backend, &vm),
            None => Ok(()),
        };
        // The runtime error is the primary failure; a screenshot failure must
        // not mask it, so report both (runtime error first).
        match (result, saved) {
            (Err(error), Err(shot)) => {
                return Err(format!("{error}\nbasic-vm: {shot}").into());
            }
            (Err(error), Ok(())) => return Err(error.into()),
            (Ok(()), Err(shot)) => return Err(shot.into()),
            (Ok(()), Ok(())) => {}
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `--test`: runs the TEST blocks of the program and prints the report.
/// Exit status: 0 when at least one test ran and all passed, 1 otherwise.
fn run_test_mode(
    path: &std::ffi::OsStr,
    filter: Option<String>,
    include_imports: bool,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let program = compile_from(&source, std::path::Path::new(path))?;
    let options = TestOptions {
        filter,
        include_imports,
    };
    let main_name = path.to_string_lossy();
    let mut output = io::stdout().lock();
    use std::io::Write;
    // Print each result as soon as its test has finished, so a slow or
    // runaway test later in the run does not hide the earlier results.
    let mut write_failed: Option<io::Error> = None;
    let report = run_tests_with(&program, &options, |result| {
        if write_failed.is_none()
            && let Err(error) =
                write!(output, "{}", result.render(&main_name)).and_then(|()| output.flush())
        {
            write_failed = Some(error);
        }
    });
    if let Some(error) = write_failed {
        return Err(error.into());
    }
    if report.results.is_empty() {
        eprintln!("basic-vm: no tests found");
        return Ok(ExitCode::FAILURE);
    }
    writeln!(output, "{}", report.summary())?;
    output.flush()?;
    Ok(if report.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn save_screenshot(
    file: &std::path::Path,
    backend: &HeadlessBackend,
    vm: &Vm,
) -> Result<(), String> {
    // Prefer the last presented frame; fall back to the visible buffer.
    let canvas = backend
        .last_frame()
        .or_else(|| vm.visible_canvas().cloned());
    let canvas = canvas.ok_or("--screenshot: the program never opened a graphics screen")?;
    let format = ImageFormat::from_path(file)
        .ok_or("--screenshot: the file must end in .ppm, .bmp or .png")?;
    fs::write(file, canvas.encode(format))
        .map_err(|error| format!("--screenshot {}: {error}", file.display()))
}
