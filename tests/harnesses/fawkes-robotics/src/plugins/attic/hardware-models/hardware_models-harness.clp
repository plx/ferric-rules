; Harness for fawkes-robotics/src/plugins/attic/hardware-models/hardware_models.clp
; Detected constructs: deftemplate: hm-component, hm-terminal-state, hm-edge, hm-transition
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
