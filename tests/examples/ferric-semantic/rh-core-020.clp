; RH-CORE-020: three-way joins preserve correlated bindings and exclude disconnected tuples.
(deffacts seed (customer alice west) (customer bob east) (order alice 5) (order bob 8) (region west active))
(defrule eligible (customer ?name ?region) (order ?name ?amount) (region ?region active) => (printout t ?name " " ?amount crlf) (assert (result ?name ?amount)))
