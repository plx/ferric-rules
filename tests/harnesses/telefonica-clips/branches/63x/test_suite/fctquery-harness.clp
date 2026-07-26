; Harness for telefonica-clips/branches/63x/test_suite/fctquery.clp
; Detected constructs: deffacts: PEOPLE; deftemplate: PERSON, FEMALE, MALE, GIRL, WOMAN, BOY, MAN, A, B, C, D, V, W, X, Y, Z, USER; defglobal: ?*list*; deffunction: count-facts/1, count-facts-2/1
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9dab1912bb989070937313a885cc0f1573352cb49617f84dddee9603e57ffd95-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9dab1912bb989070937313a885cc0f1573352cb49617f84dddee9603e57ffd95|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9dab1912bb989070937313a885cc0f1573352cb49617f84dddee9603e57ffd95|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9dab1912bb989070937313a885cc0f1573352cb49617f84dddee9603e57ffd95|COMPLETE" crlf))
