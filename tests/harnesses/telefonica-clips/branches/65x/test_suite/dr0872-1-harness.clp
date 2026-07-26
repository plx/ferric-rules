; Harness for telefonica-clips/branches/65x/test_suite/dr0872-1.clp
; Detected constructs: deffunction: testUnmatched/0
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ea1723ba58efc0e72ee8597a2850f5f0ed09065e77912ac96c93189b269172a2-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ea1723ba58efc0e72ee8597a2850f5f0ed09065e77912ac96c93189b269172a2|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ea1723ba58efc0e72ee8597a2850f5f0ed09065e77912ac96c93189b269172a2|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ea1723ba58efc0e72ee8597a2850f5f0ed09065e77912ac96c93189b269172a2|COMPLETE" crlf))
