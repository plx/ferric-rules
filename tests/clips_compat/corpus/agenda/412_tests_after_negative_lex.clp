(deffacts seed (u 3) (b 2))
(defrule not-base (declare (salience 30)) (u ?x) (not (absent)) => (printout t NOT-BASE crlf))
(defrule exists-base (declare (salience 20)) (u ?x) (exists (b ?)) => (printout t EXISTS-BASE crlf))
(defrule ncc-base (declare (salience 10)) (u ?x) (not (and (absent) (missing))) => (printout t NCC-BASE crlf))
(defrule install (declare (salience 1000)) =>
  (build "(defrule not-tested (declare (salience 30)) (u ?x) (not (absent)) (test (> ?x 0)) => (printout t NOT-TESTED crlf))")
  (build "(defrule exists-tested (declare (salience 20)) (u ?x) (exists (b ?)) (test (> ?x 0)) => (printout t EXISTS-TESTED crlf))")
  (build "(defrule ncc-tested (declare (salience 10)) (u ?x) (not (and (absent) (missing))) (test (> ?x 0)) => (printout t NCC-TESTED crlf))"))
