; Harness for telefonica-clips/branches/63x/test_suite/dr0872-1.clp
; Detected constructs: deffunction: testUnmatched/0
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-0aa318303a94a62721bafb4175f6cbd98bf1d34107e6544347b438a09722fad4-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-0aa318303a94a62721bafb4175f6cbd98bf1d34107e6544347b438a09722fad4|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0aa318303a94a62721bafb4175f6cbd98bf1d34107e6544347b438a09722fad4|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0aa318303a94a62721bafb4175f6cbd98bf1d34107e6544347b438a09722fad4|COMPLETE" crlf))
