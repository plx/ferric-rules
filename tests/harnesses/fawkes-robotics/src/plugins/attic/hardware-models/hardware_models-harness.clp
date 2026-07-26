; Harness for fawkes-robotics/src/plugins/attic/hardware-models/hardware_models.clp
; Detected constructs: deftemplate: hm-component, hm-terminal-state, hm-edge, hm-transition
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-5493ca1ef677926c2dd3cb54a15d7561f2c5de6cc9db1ea965ca786c28e537ab-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-5493ca1ef677926c2dd3cb54a15d7561f2c5de6cc9db1ea965ca786c28e537ab|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-5493ca1ef677926c2dd3cb54a15d7561f2c5de6cc9db1ea965ca786c28e537ab|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-5493ca1ef677926c2dd3cb54a15d7561f2c5de6cc9db1ea965ca786c28e537ab|COMPLETE" crlf))
