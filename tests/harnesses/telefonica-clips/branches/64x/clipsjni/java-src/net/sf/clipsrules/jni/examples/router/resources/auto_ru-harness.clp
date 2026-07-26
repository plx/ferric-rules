; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-93dc78ac5ab9fc179142b5e935d0eba2d8639a43b469e9795ec28aa5eae69a83-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-93dc78ac5ab9fc179142b5e935d0eba2d8639a43b469e9795ec28aa5eae69a83|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-93dc78ac5ab9fc179142b5e935d0eba2d8639a43b469e9795ec28aa5eae69a83|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-93dc78ac5ab9fc179142b5e935d0eba2d8639a43b469e9795ec28aa5eae69a83|COMPLETE" crlf))
