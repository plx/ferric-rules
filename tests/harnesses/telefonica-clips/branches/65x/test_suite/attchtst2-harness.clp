; Harness for telefonica-clips/branches/65x/test_suite/attchtst2.clp
; Detected constructs: deftemplate: a, b, c, d, e, f, g, h
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-e4960b9addc89f30c3f343044352b907197bff6b59414c1c244b37fde9e1d6c8-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-e4960b9addc89f30c3f343044352b907197bff6b59414c1c244b37fde9e1d6c8|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-e4960b9addc89f30c3f343044352b907197bff6b59414c1c244b37fde9e1d6c8|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-e4960b9addc89f30c3f343044352b907197bff6b59414c1c244b37fde9e1d6c8|COMPLETE" crlf))
