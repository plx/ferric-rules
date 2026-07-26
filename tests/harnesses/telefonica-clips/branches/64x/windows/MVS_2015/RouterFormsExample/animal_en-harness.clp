; Harness for telefonica-clips/branches/64x/windows/MVS_2015/RouterFormsExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-51dca8d4fd024686e1050ca0fea975872bd7a43565de104fafd2e807113a9ea0-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-51dca8d4fd024686e1050ca0fea975872bd7a43565de104fafd2e807113a9ea0|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-51dca8d4fd024686e1050ca0fea975872bd7a43565de104fafd2e807113a9ea0|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-51dca8d4fd024686e1050ca0fea975872bd7a43565de104fafd2e807113a9ea0|COMPLETE" crlf))
