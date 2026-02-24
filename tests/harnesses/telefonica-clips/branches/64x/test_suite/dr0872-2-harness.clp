; Harness for telefonica-clips/branches/64x/test_suite/dr0872-2.clp
; Detected constructs: defmethod: foo
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
