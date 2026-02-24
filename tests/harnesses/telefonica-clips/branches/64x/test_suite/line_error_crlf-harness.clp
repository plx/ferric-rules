; Harness for telefonica-clips/branches/64x/test_suite/line_error_crlf.clp
; Detected constructs: deffacts: points; deftemplate: point
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
