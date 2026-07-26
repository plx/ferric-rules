; Harness for telefonica-clips/branches/64x/windows/MVS_2017/AnimalWPFExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-fca1d25d01eb117b6b16c405fbe5ae4b246a41a983da8334792b5392baf35b3c-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-fca1d25d01eb117b6b16c405fbe5ae4b246a41a983da8334792b5392baf35b3c|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-fca1d25d01eb117b6b16c405fbe5ae4b246a41a983da8334792b5392baf35b3c|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-fca1d25d01eb117b6b16c405fbe5ae4b246a41a983da8334792b5392baf35b3c|COMPLETE" crlf))
