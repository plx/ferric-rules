;; Issue #323: unrestricted defmethod parameter compatibility.
(defglobal ?*calls* = 0)
(defgeneric record)

;; BEGIN METHODS
(defmethod record (?x)
  (bind ?*calls* (+ ?*calls* 1))
  (printout t "body:" ?x ":" ?*calls* crlf)
  ?x)
;; END METHODS

(defrule probe
  =>
  (record 17)
  (record blue)
  (printout t "calls:" ?*calls* crlf))
