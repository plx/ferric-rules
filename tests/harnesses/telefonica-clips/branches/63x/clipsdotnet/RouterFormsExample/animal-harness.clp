; Harness for telefonica-clips/branches/63x/clipsdotnet/RouterFormsExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-81d3bc0050e5e3f333afe9198399c1ed1fcee8107de650b9171c8c35f071c5ff-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-81d3bc0050e5e3f333afe9198399c1ed1fcee8107de650b9171c8c35f071c5ff|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-81d3bc0050e5e3f333afe9198399c1ed1fcee8107de650b9171c8c35f071c5ff|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-81d3bc0050e5e3f333afe9198399c1ed1fcee8107de650b9171c8c35f071c5ff|COMPLETE" crlf))
