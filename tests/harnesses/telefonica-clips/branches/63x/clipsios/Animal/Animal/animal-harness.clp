; Harness for telefonica-clips/branches/63x/clipsios/Animal/Animal/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-79218fdeb1af1692ba996ad9e97ed0289833548abdf4ac95b208b01b83a38c8b-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-79218fdeb1af1692ba996ad9e97ed0289833548abdf4ac95b208b01b83a38c8b|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-79218fdeb1af1692ba996ad9e97ed0289833548abdf4ac95b208b01b83a38c8b|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-79218fdeb1af1692ba996ad9e97ed0289833548abdf4ac95b208b01b83a38c8b|COMPLETE" crlf))
