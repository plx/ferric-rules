; Harness for clips-official/test_suite/fctquery.clp
; Detected constructs: deffacts: PEOPLE; deftemplate: PERSON, FEMALE, MALE, GIRL, WOMAN, BOY, MAN, A, B, C, D, V, W, X, Y, Z, USER; defglobal: ?*list*; deffunction: count-facts/1, count-facts-2/1
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
