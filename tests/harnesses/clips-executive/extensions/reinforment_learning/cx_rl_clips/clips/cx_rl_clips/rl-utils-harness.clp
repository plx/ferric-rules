; Harness for clips-executive/extensions/reinforment_learning/cx_rl_clips/clips/cx_rl_clips/rl-utils.clp
; Detected constructs: defglobal: ?*CX-RL-LOG-LEVEL*; deffunction: cx-rl-create-slot-value-string/1, cx-rl-create-observation-string/1
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ce364f1f5119c1f3763820975b0f71e24f431e6dedfa4ef675e4fd2c16cf3d73-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ce364f1f5119c1f3763820975b0f71e24f431e6dedfa4ef675e4fd2c16cf3d73|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ce364f1f5119c1f3763820975b0f71e24f431e6dedfa4ef675e4fd2c16cf3d73|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ce364f1f5119c1f3763820975b0f71e24f431e6dedfa4ef675e4fd2c16cf3d73|COMPLETE" crlf))
