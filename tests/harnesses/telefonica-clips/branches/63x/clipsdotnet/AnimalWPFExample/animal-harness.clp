; Harness for telefonica-clips/branches/63x/clipsdotnet/AnimalWPFExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-a5ee57a702a4d04d9d950b5918ea6d69fd7cb8123b9ba26994c9d26f5eb7958d-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-a5ee57a702a4d04d9d950b5918ea6d69fd7cb8123b9ba26994c9d26f5eb7958d|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a5ee57a702a4d04d9d950b5918ea6d69fd7cb8123b9ba26994c9d26f5eb7958d|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a5ee57a702a4d04d9d950b5918ea6d69fd7cb8123b9ba26994c9d26f5eb7958d|COMPLETE" crlf))
