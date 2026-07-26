; Harness for telefonica-clips/branches/65x/test_suite/fctquery.clp
; Detected constructs: deffacts: PEOPLE; deftemplate: PERSON, FEMALE, MALE, GIRL, WOMAN, BOY, MAN, A, B, C, D, V, W, X, Y, Z, USER; defglobal: ?*list*; deffunction: count-facts/1, count-facts-2/1
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-704ed8f21910bbf71a1240f7234f4e6b6c9559b506a0d0d8e26fea80628635f9-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-704ed8f21910bbf71a1240f7234f4e6b6c9559b506a0d0d8e26fea80628635f9|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-704ed8f21910bbf71a1240f7234f4e6b6c9559b506a0d0d8e26fea80628635f9|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-704ed8f21910bbf71a1240f7234f4e6b6c9559b506a0d0d8e26fea80628635f9|COMPLETE" crlf))
