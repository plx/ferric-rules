; Harness for telefonica-clips/branches/65x/test_suite/attchtst4.clp
; Detected constructs: deftemplate: a, b, c, d, e, f, g, h
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
