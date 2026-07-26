; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-482e07f6fa1682b019b0b35255f502c0f4cea22463c190eed93b028ca6389705-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-482e07f6fa1682b019b0b35255f502c0f4cea22463c190eed93b028ca6389705|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-482e07f6fa1682b019b0b35255f502c0f4cea22463c190eed93b028ca6389705|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-482e07f6fa1682b019b0b35255f502c0f4cea22463c190eed93b028ca6389705|COMPLETE" crlf))
