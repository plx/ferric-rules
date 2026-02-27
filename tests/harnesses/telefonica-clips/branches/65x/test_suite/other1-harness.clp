; Harness for telefonica-clips/branches/65x/test_suite/other1.clp
; Detected constructs: deffacts: wine-rules, initial-goal; deftemplate: rule
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
