; Harness for clips-official/test_suite/tempbug.clp
; Detected constructs: defglobal: ?*q*, ?*x*
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
