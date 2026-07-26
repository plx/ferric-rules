; Harness for telefonica-clips/branches/64x/test_suite/fctquery.clp
; Detected constructs: deffacts: PEOPLE; deftemplate: PERSON, FEMALE, MALE, GIRL, WOMAN, BOY, MAN, A, B, C, D, V, W, X, Y, Z, USER; defglobal: ?*list*; deffunction: count-facts/1, count-facts-2/1
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-a59d301c5bc172071f6269cb27f71cd2e4dea1014c1a7e985764f45042e13977-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-a59d301c5bc172071f6269cb27f71cd2e4dea1014c1a7e985764f45042e13977|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a59d301c5bc172071f6269cb27f71cd2e4dea1014c1a7e985764f45042e13977|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a59d301c5bc172071f6269cb27f71cd2e4dea1014c1a7e985764f45042e13977|COMPLETE" crlf))
