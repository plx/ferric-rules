; Harness for telefonica-clips/branches/63x/clipsdotnet/AnimalFormsExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-885bef3fc3b49f5f70c31a9ab188310b9939b28c21ec2734577b09818218e5f9-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-885bef3fc3b49f5f70c31a9ab188310b9939b28c21ec2734577b09818218e5f9|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-885bef3fc3b49f5f70c31a9ab188310b9939b28c21ec2734577b09818218e5f9|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-885bef3fc3b49f5f70c31a9ab188310b9939b28c21ec2734577b09818218e5f9|COMPLETE" crlf))
