; Harness for rcll-refbox/src/games/llsf2014/config.clp
; Detected constructs: deftemplate: confval
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
