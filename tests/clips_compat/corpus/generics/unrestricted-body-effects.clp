;; Issue #323: unrestricted defmethod parameter compatibility.
(defglobal ?*calls* = 0)
(defgeneric record)

(defmethod record (?x)
  (bind ?*calls* (+ ?*calls* 1))
  (printout t "body:" ?x ":" ?*calls* crlf)
  ?x)

(defrule probe
  =>
  (record 17)
  (record blue)
  (printout t "calls:" ?*calls* crlf))
