;; Every parsed field retains alternatives, predicates, and return-value constraints.
(deftemplate item (slot low) (multislot tags) (slot high))
(defglobal ?*alternatives* = 0 ?*predicate* = 0 ?*returned* = 0 ?*siblings* = 0)
(deffacts input
  (item (low 1) (tags head 2 tail) (high 3))
  (item (low 4) (tags head 6 end) (high 5))
  (item (low 7) (tags head 9 missing) (high 8)))
(defrule alternatives
  (item (tags head $?middle tail|end))
  => (bind ?*alternatives* (+ ?*alternatives* 1)))
(defrule predicate
  (item (tags head ?value&:(> ?value 5) $?rest))
  => (bind ?*predicate* (+ ?*predicate* 1)))
(defrule returned
  (item (low ?base) (tags head =(+ ?base 1) $?tail))
  => (bind ?*returned* (+ ?*returned* 1)))
(defrule scalar-siblings
  (item (tags $?values) (low ?lower) (high ?higher&:(> ?higher ?lower)))
  => (bind ?*siblings* (+ ?*siblings* 1)))
(defrule summary (declare (salience -10))
  => (printout t ?*alternatives* ":" ?*predicate* ":" ?*returned* ":" ?*siblings* crlf))
