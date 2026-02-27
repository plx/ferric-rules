; Harness for rcll-refbox/src/games/rcll/priorities.clp
; Detected constructs: defglobal: ?*PRIORITY_FIRST*, ?*PRIORITY_HIGHER*, ?*PRIORITY_HIGH*, ?*PRIORITY_CLEANUP*, ?*PRIORITY_LAST*
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
