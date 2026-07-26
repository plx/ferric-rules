; Harness for telefonica-clips/branches/63x/test_suite/globlerr.clp
; Detected constructs: defglobal: ?*x*, ?*r*, ?*y*, ?*z*, ?*w*, ?*q*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-6ff705f99fb202b706c4358af635d1c31fa2a7bfd3339b62ba42f164a28ea27e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-6ff705f99fb202b706c4358af635d1c31fa2a7bfd3339b62ba42f164a28ea27e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-6ff705f99fb202b706c4358af635d1c31fa2a7bfd3339b62ba42f164a28ea27e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-6ff705f99fb202b706c4358af635d1c31fa2a7bfd3339b62ba42f164a28ea27e|COMPLETE" crlf))
