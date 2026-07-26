; Harness for clips-official/test_suite/fctquery.clp
; Detected constructs: deffacts: PEOPLE; deftemplate: PERSON, FEMALE, MALE, GIRL, WOMAN, BOY, MAN, A, B, C, D, V, W, X, Y, Z, USER; defglobal: ?*list*; deffunction: count-facts/1, count-facts-2/1
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-d94dd519919d40d998e0ca14607271a1b16a11df720ea32745d00a5d3387a16b-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-d94dd519919d40d998e0ca14607271a1b16a11df720ea32745d00a5d3387a16b|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d94dd519919d40d998e0ca14607271a1b16a11df720ea32745d00a5d3387a16b|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d94dd519919d40d998e0ca14607271a1b16a11df720ea32745d00a5d3387a16b|COMPLETE" crlf))
