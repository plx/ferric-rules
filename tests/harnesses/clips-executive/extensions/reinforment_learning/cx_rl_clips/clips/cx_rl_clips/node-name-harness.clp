; Harness for clips-executive/extensions/reinforment_learning/cx_rl_clips/clips/cx_rl_clips/node-name.clp
; Detected constructs: defglobal: ?*CX-RL-NODE-NAME*
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
