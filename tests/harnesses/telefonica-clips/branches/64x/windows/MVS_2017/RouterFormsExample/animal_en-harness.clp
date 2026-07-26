; Harness for telefonica-clips/branches/64x/windows/MVS_2017/RouterFormsExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-377f545161711527cfba32134571d96109736dde36f46a4f56f9c626d18579a9-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-377f545161711527cfba32134571d96109736dde36f46a4f56f9c626d18579a9|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-377f545161711527cfba32134571d96109736dde36f46a4f56f9c626d18579a9|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-377f545161711527cfba32134571d96109736dde36f46a4f56f9c626d18579a9|COMPLETE" crlf))
