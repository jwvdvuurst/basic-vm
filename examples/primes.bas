REM Go through numbers and either state that a number is prime or give the factors if it is not
LET PRIMES = LIST(2)
FOR N = 3 TO 100
    LET IS_PRIME = 1
    FOR EACH P IN PRIMES
        LET M = MOD(N,P)
        IF M <> 0 THEN 100
        LET IS_PRIME = 0
        PRINT N; " is not prime, divisible by "; P
        GOTO 200
100 NEXT P
200 IF IS_PRIME THEN APPEND PRIMES, N
    IF IS_PRIME THEN PRINT N; " is prime"
NEXT N