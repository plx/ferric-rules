; Harness for telefonica-clips/branches/64x/test_suite/dr0872-1.clp
; Detected constructs: deffunction: testUnmatched/0
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-8dd1ca82ac3b55fd0f225f999e8f5ef5d2f28b6db3ae1e91ffc17393cce110e3-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-8dd1ca82ac3b55fd0f225f999e8f5ef5d2f28b6db3ae1e91ffc17393cce110e3|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8dd1ca82ac3b55fd0f225f999e8f5ef5d2f28b6db3ae1e91ffc17393cce110e3|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8dd1ca82ac3b55fd0f225f999e8f5ef5d2f28b6db3ae1e91ffc17393cce110e3|COMPLETE" crlf))
