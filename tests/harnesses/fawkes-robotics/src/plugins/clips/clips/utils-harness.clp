; Harness for fawkes-robotics/src/plugins/clips/clips/utils.clp
; Detected constructs: defglobal: ?*DEBUG*; deffunction: debug-set-level/1, debug/1, set-eq/2, set-diff/2, is-even-int/1, is-odd-int/1, str-replace/3, str-prefix/2, str-split/2
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
