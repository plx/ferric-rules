; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/AnimalFormsExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-49dc00da6fdc98b63521596c8f378069513fa0e4b89f83374df8230a977a2f9f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-49dc00da6fdc98b63521596c8f378069513fa0e4b89f83374df8230a977a2f9f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-49dc00da6fdc98b63521596c8f378069513fa0e4b89f83374df8230a977a2f9f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-49dc00da6fdc98b63521596c8f378069513fa0e4b89f83374df8230a977a2f9f|COMPLETE" crlf))
