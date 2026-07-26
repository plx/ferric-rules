; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/RouterFormsExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-dda776409e782794bfe76907c2b9ec0c67fb649a30a615e0905839b844a6b326-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-dda776409e782794bfe76907c2b9ec0c67fb649a30a615e0905839b844a6b326|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-dda776409e782794bfe76907c2b9ec0c67fb649a30a615e0905839b844a6b326|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-dda776409e782794bfe76907c2b9ec0c67fb649a30a615e0905839b844a6b326|COMPLETE" crlf))
