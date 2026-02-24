; Harness for telefonica-clips/branches/65x/test_suite/gnrcovl.clp
; Detected constructs: defglobal: ?*success*; deffunction: alt-str-cat/1, print-result/2, testit/0; defmethod: sym-cat
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
