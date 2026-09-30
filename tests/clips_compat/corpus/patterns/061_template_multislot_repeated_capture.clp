;; Repeated variables compare the complete subsequences within one multislot.
(deftemplate item (multislot tags))
(deffacts input (item (tags a marker a)) (item (tags a marker b)) (item (tags marker)))
(defglobal ?*count* = 0)
(defrule repeated
  (item (tags $?same marker $?same))
  => (bind ?*count* (+ ?*count* 1)))
(defrule summary (declare (salience -10)) => (printout t ?*count* crlf))
