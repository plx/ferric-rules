; Harness for clips-executive/cx_plugins/config_plugin/clips/ff-config.clp
; Detected constructs: deftemplate: confval
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
