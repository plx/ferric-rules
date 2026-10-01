;; Issue #323: unrestricted defmethod parameter compatibility.
(defgeneric mixed)

(defmethod mixed (?first (?n INTEGER) ?last)
  (str-cat ?first ":" ?n ":" ?last))

(defrule probe
  =>
  (printout t (mixed blue 7 "tail") crlf (mixed "head" 3 green) crlf))
