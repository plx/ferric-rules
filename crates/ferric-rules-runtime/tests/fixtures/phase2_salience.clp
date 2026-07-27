;; Phase 2 salience fixture
;; Tests: salience-based conflict resolution
(deffacts startup
    (trigger))

(defrule low-priority
    (trigger)
    =>
    (assert (fired-low)))

(defrule high-priority
    (declare (salience 10))
    (trigger)
    =>
    (assert (fired-high)))
