; Harness for telefonica-clips/branches/63x/test_suite/globlerr.clp
; Detected constructs: defglobal: ?*x*, ?*r*, ?*y*, ?*z*, ?*w*, ?*q*
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
