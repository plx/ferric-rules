; Harness for telefonica-clips/branches/64x/windows/MVS_2017/RouterWPFExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-8bc3ca49d23845a5ae4d869e0176ee738e1cba7c01322537b54668560b46ed06-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-8bc3ca49d23845a5ae4d869e0176ee738e1cba7c01322537b54668560b46ed06|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8bc3ca49d23845a5ae4d869e0176ee738e1cba7c01322537b54668560b46ed06|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8bc3ca49d23845a5ae4d869e0176ee738e1cba7c01322537b54668560b46ed06|COMPLETE" crlf))
