; Harness for fawkes-robotics/src/plugins/clips/clips/utils.clp
; Detected constructs: defglobal: ?*DEBUG*; deffunction: debug-set-level/1, debug/1, set-eq/2, set-diff/2, is-even-int/1, is-odd-int/1, str-replace/3, str-prefix/2, str-split/2
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-bdf70769ae1aa1c007b72d40be43bf7131de0d6c0e23c752f337af4d54659bb6-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-bdf70769ae1aa1c007b72d40be43bf7131de0d6c0e23c752f337af4d54659bb6|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bdf70769ae1aa1c007b72d40be43bf7131de0d6c0e23c752f337af4d54659bb6|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bdf70769ae1aa1c007b72d40be43bf7131de0d6c0e23c752f337af4d54659bb6|COMPLETE" crlf))
