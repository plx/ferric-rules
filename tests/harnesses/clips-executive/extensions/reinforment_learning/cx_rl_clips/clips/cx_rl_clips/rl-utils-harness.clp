; Harness for clips-executive/extensions/reinforment_learning/cx_rl_clips/clips/cx_rl_clips/rl-utils.clp
; Detected constructs: defglobal: ?*CX-RL-LOG-LEVEL*; deffunction: cx-rl-create-slot-value-string/1, cx-rl-create-observation-string/1
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
