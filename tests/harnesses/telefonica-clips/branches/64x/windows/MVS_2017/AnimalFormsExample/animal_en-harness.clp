; Harness for telefonica-clips/branches/64x/windows/MVS_2017/AnimalFormsExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ddc6ded7b26697d57541fe587e243ddb143ecb5f24932017b9e0b8bc650776d5-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ddc6ded7b26697d57541fe587e243ddb143ecb5f24932017b9e0b8bc650776d5|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ddc6ded7b26697d57541fe587e243ddb143ecb5f24932017b9e0b8bc650776d5|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ddc6ded7b26697d57541fe587e243ddb143ecb5f24932017b9e0b8bc650776d5|COMPLETE" crlf))
