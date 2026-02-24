; Harness for telefonica-clips/branches/63x/test_suite/dr0872-1.clp
; Detected constructs: deffunction: testUnmatched/0
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
