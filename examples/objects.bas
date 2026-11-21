REM Objects: CLASS = record + methods, SELF, constructors, composition.

CLASS Engine
  horsepower AS INTEGER = 100
  running AS BOOLEAN

  FUNCTION NEW(PARAMETER hp AS INTEGER = 100)
    SELF.horsepower = hp
  FEND

  FUNCTION Start$() AS STRING
    SELF.running = TRUE
    Start$ = "engine " + STR$(SELF.horsepower) + "hp started"
  FEND
END CLASS

CLASS Car
  name$ AS STRING
  engine AS Engine
  trips AS INTEGER

  FUNCTION NEW(PARAMETER n$ AS STRING, PARAMETER hp AS INTEGER = 100)
    SELF.name$ = n$
    SELF.engine = NEW Engine(hp)
  FEND

  FUNCTION Drive$(PARAMETER km AS INTEGER) AS STRING
    IF NOT SELF.engine.running THEN
      PRINT SELF.engine.Start$()
    END IF
    SELF.trips = SELF.trips + 1
    Drive$ = SELF.name$ + " drove " + STR$(km) + "km (trip " + STR$(SELF.trips) + ")"
  FEND

  FUNCTION Describe$() AS STRING
    Describe$ = SELF.name$ + " with " + STR$(SELF.engine.horsepower) + "hp"
  FEND
END CLASS

DIM a AS Car = NEW Car("Mini", 75)
DIM b AS Car = NEW Car(n$ := "Truck", hp := 400)
PRINT a.Describe$()
PRINT b.Describe$()
PRINT a.Drive$(10)
PRINT a.Drive$(5)
PRINT b.Drive$(300)
PRINT TYPENAME$(a); " "; TYPEOF(a)

REM Objects live in collections; the method is found by the receiver's class.
DIM garage AS LIST
APPEND garage, a
APPEND garage, b
FOR EACH c IN garage
  CALL c.Drive$(1)
NEXT
PRINT a.trips; " "; b.trips
