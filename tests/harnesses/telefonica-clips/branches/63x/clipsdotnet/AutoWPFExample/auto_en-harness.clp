; Harness for telefonica-clips/branches/63x/clipsdotnet/AutoWPFExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-2943215b41519299d0d83fa510682bb75f2d1d6a1e50d561aa4fb250c6541788-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-2943215b41519299d0d83fa510682bb75f2d1d6a1e50d561aa4fb250c6541788|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2943215b41519299d0d83fa510682bb75f2d1d6a1e50d561aa4fb250c6541788|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2943215b41519299d0d83fa510682bb75f2d1d6a1e50d561aa4fb250c6541788|COMPLETE" crlf))
