; Harness for telefonica-clips/branches/63x/test_suite/dr0872-2.clp
; Detected constructs: defmethod: foo
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-bfca274b10be9b9a6e85ebd676167249884bf3e66494c787e3458db56512c8b8-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-bfca274b10be9b9a6e85ebd676167249884bf3e66494c787e3458db56512c8b8|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bfca274b10be9b9a6e85ebd676167249884bf3e66494c787e3458db56512c8b8|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bfca274b10be9b9a6e85ebd676167249884bf3e66494c787e3458db56512c8b8|COMPLETE" crlf))
