; Harness for telefonica-clips/branches/64x/windows/MVS_2017/AnimalWPFExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-68788c25edc711bb708befaa9fc7afef56daf37fe7779c6b388f651506d71c06-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-68788c25edc711bb708befaa9fc7afef56daf37fe7779c6b388f651506d71c06|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-68788c25edc711bb708befaa9fc7afef56daf37fe7779c6b388f651506d71c06|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-68788c25edc711bb708befaa9fc7afef56daf37fe7779c6b388f651506d71c06|COMPLETE" crlf))
