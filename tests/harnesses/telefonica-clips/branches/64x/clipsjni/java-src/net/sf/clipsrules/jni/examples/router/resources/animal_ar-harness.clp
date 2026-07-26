; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-6e72552ea4ca8006fd9f5227426eab922ff4883dda67f590b87dd63e37811c7c-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-6e72552ea4ca8006fd9f5227426eab922ff4883dda67f590b87dd63e37811c7c|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-6e72552ea4ca8006fd9f5227426eab922ff4883dda67f590b87dd63e37811c7c|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-6e72552ea4ca8006fd9f5227426eab922ff4883dda67f590b87dd63e37811c7c|COMPLETE" crlf))
