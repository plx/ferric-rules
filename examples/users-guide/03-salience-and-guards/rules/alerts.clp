(defrule alarm-on-fire
    (declare (salience 100))
    (sensor smoke high)
    (not (decision-made))
    =>
    (assert (alert evacuate))
    (assert (decision-made)))

(defrule warn-on-heat
    (declare (salience 50))
    (sensor temperature ?t)
    (test (> ?t 90))
    (not (decision-made))
    =>
    (assert (alert high-temp))
    (assert (decision-made)))

(defrule monitor
    (declare (salience 10))
    (initial-fact)
    (not (decision-made))
    =>
    (assert (alert none))
    (assert (decision-made)))
