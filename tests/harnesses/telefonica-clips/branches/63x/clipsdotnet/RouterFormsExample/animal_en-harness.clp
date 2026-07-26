; Harness for telefonica-clips/branches/63x/clipsdotnet/RouterFormsExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9d5c90dbf627e8633e0a65c856018386f34bcb35d90aebe8ac3b3aa0b0d374d1-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9d5c90dbf627e8633e0a65c856018386f34bcb35d90aebe8ac3b3aa0b0d374d1|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9d5c90dbf627e8633e0a65c856018386f34bcb35d90aebe8ac3b3aa0b0d374d1|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9d5c90dbf627e8633e0a65c856018386f34bcb35d90aebe8ac3b3aa0b0d374d1|COMPLETE" crlf))
