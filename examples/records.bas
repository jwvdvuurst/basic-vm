REM Records: TYPE ... END TYPE, NEW, fields, COPY, NOTHING and linked nodes.
OPTION EXPLICIT

TYPE Person
  name$ AS STRING = "anon"
  age AS INTEGER
  boss AS Person
END TYPE

DIM ann AS Person
ann.name$ = "Ann"
ann.age = 41
DIM bob AS Person = NEW Person(name$ = "Bob", age = 29)
bob.boss = ann

PRINT bob.name$; " is "; bob.age; ", boss: "; bob.boss.name$
PRINT TYPEOF(bob); " "; TYPENAME$(bob)
PRINT "no boss for Ann: "; ann.boss IS NOTHING

REM Aliasing versus COPY
DIM alias AS Person
alias = bob
alias.age = 30
DIM clone AS Person
clone = COPY(bob)
clone.age = 99
PRINT bob.age; " "; clone.age; " "; alias = bob; " "; clone = bob

REM Records in a list, sorted by age with a simple exchange sort
DIM people AS LIST
APPEND people, bob
APPEND people, ann
APPEND people, clone
DIM i AS INTEGER
DIM j AS INTEGER
FOR i = 1 TO LEN(people) - 1
  FOR j = 1 TO LEN(people) - i
    IF people[j].age > people[j + 1].age THEN
      DIM tmp AS Person
      tmp = people[j]
      people[j] = people[j + 1]
      people[j + 1] = tmp
    END IF
  NEXT j
NEXT i
FOR i = 1 TO LEN(people)
  PRINT people[i].name$; " "; people[i].age
NEXT i

REM Linked list of nodes
TYPE Node
  value AS INTEGER
  next AS Node
END TYPE
DIM head AS Node = NOTHING
DIM n AS Node
FOR i = 1 TO 3
  n = NEW Node(value = i)
  n.next = head
  head = n
NEXT i
n = head
WHILE NOT (n IS NOTHING)
  PRINT "node "; n.value
  n = n.next
WEND

REM Fields by name
DIM names AS LIST = FIELDS(bob)
FOR EACH f$ IN names
  PRINT f$; " ";
NEXT
PRINT
PRINT GETFIELD(bob, "age")
