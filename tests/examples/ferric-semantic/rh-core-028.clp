; RH-CORE-028: modifying a template fact cancels its stale join and enables its new join.
(deftemplate account (slot id) (slot status))
(deffacts seed (account (id a) (status pending)) (permission active))
(defrule promote (declare (salience 100)) ?a <- (account (id a) (status pending)) => (modify ?a (status active)))
(defrule stale (account (status pending)) => (printout t "incorrect" crlf))
(defrule active (account (id ?id) (status ?s)) (permission ?s) => (printout t "active " ?id crlf) (assert (result ?id)))
