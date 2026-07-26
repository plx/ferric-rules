; Harness for telefonica-clips/branches/64x/test_suite/tempbug.clp
; Detected constructs: defglobal: ?*q*, ?*x*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-341df3be1b5eb77b6c8230f73cf2de5e01681925a6376a0f968243c21c900b0f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-341df3be1b5eb77b6c8230f73cf2de5e01681925a6376a0f968243c21c900b0f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-341df3be1b5eb77b6c8230f73cf2de5e01681925a6376a0f968243c21c900b0f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-341df3be1b5eb77b6c8230f73cf2de5e01681925a6376a0f968243c21c900b0f|COMPLETE" crlf))
