; Harness for telefonica-clips/branches/64x/windows/MVS_2017/RouterWPFExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-4d9e5b19b170fa517c38a2d1519fe09161ef444cecea78867d259bc4f209d29f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-4d9e5b19b170fa517c38a2d1519fe09161ef444cecea78867d259bc4f209d29f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-4d9e5b19b170fa517c38a2d1519fe09161ef444cecea78867d259bc4f209d29f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-4d9e5b19b170fa517c38a2d1519fe09161ef444cecea78867d259bc4f209d29f|COMPLETE" crlf))
