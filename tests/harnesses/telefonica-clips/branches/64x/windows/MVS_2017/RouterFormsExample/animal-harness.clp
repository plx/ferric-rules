; Harness for telefonica-clips/branches/64x/windows/MVS_2017/RouterFormsExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
