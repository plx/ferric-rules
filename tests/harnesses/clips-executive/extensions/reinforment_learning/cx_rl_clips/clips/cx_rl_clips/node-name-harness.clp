; Harness for clips-executive/extensions/reinforment_learning/cx_rl_clips/clips/cx_rl_clips/node-name.clp
; Detected constructs: defglobal: ?*CX-RL-NODE-NAME*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-1e282f284dae7df06b98be68a0e2b651b1f2ecf16227d8423949f36005add24f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-1e282f284dae7df06b98be68a0e2b651b1f2ecf16227d8423949f36005add24f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1e282f284dae7df06b98be68a0e2b651b1f2ecf16227d8423949f36005add24f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1e282f284dae7df06b98be68a0e2b651b1f2ecf16227d8423949f36005add24f|COMPLETE" crlf))
