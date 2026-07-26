; Harness for telefonica-clips/branches/64x/windows/MVS_2017/AnimalFormsExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-0c2b47ef83bc3b21809d307a9e5d58320124392d78b64ac18a671275fa03c77b-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-0c2b47ef83bc3b21809d307a9e5d58320124392d78b64ac18a671275fa03c77b|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0c2b47ef83bc3b21809d307a9e5d58320124392d78b64ac18a671275fa03c77b|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0c2b47ef83bc3b21809d307a9e5d58320124392d78b64ac18a671275fa03c77b|COMPLETE" crlf))
