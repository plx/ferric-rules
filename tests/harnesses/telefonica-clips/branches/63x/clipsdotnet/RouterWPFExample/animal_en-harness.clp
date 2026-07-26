; Harness for telefonica-clips/branches/63x/clipsdotnet/RouterWPFExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-184428e6d3c993e3a8420eb217dc5dea46e782a19e41f76b831f0531655861bc-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-184428e6d3c993e3a8420eb217dc5dea46e782a19e41f76b831f0531655861bc|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-184428e6d3c993e3a8420eb217dc5dea46e782a19e41f76b831f0531655861bc|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-184428e6d3c993e3a8420eb217dc5dea46e782a19e41f76b831f0531655861bc|COMPLETE" crlf))
