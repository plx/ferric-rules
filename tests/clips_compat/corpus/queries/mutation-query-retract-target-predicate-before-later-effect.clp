(defglobal ?*g* = 0)
(deftemplate item (slot kind))
(deffacts seed (item (kind first)) (item (kind second)))
(deffunction mark (?address) (bind ?*g* 1) ?address)
(defrule observer
  (not (item (kind first)))
  (test (= ?*g* 0))
  => (printout t "observer:" ?*g* crlf))
(defrule controller
  (declare (salience 100))
  ?first <- (item (kind first))
  ?second <- (item (kind second))
  =>
  (retract ?first (mark ?second))
  (printout t "after:" ?*g* crlf))
