; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/RouterWPFExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-56dc8c8af27ef039314883183f31c7168ca894aecfd4bdd9f4e227dcbb2a8c39-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-56dc8c8af27ef039314883183f31c7168ca894aecfd4bdd9f4e227dcbb2a8c39|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-56dc8c8af27ef039314883183f31c7168ca894aecfd4bdd9f4e227dcbb2a8c39|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-56dc8c8af27ef039314883183f31c7168ca894aecfd4bdd9f4e227dcbb2a8c39|COMPLETE" crlf))
