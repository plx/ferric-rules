; Harness for clips-official/test_suite/gnrcprc.clp
; Detected constructs: deffunction: testit/0; defgeneric: mv-slot-replace, class-slots; defmethod: t1, t2, t3, mv-slot-replace, class-slots
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
