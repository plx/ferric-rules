; Harness for clips-executive/extensions/pddl/cx_pddl_bringup/clips/cx_pddl_bringup/deftemplate-overrides.clp
; Detected constructs: deftemplate: pddl-action
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
