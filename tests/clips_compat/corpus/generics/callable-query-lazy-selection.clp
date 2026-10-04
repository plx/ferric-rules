(defglobal ?*queries* = 0)
(deffunction mark (?x) (bind ?*queries* (+ ?*queries* 1)) TRUE)
(defgeneric choose)
(defmethod choose ((?x NUMBER (mark ?x))) number)
(defmethod choose ((?x INTEGER)) integer)
(defrule run =>
  (printout t (choose 1) ":" ?*queries* crlf)
  (printout t (choose 1.5) ":" ?*queries* crlf))
