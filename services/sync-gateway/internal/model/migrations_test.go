package model

import (
	"database/sql"
	"fmt"
	"os"
	"os/exec"
	"testing"

	_ "github.com/jackc/pgx/v5/stdlib"
)

func TestMigrations(t *testing.T) {
	dbURL := os.Getenv("TEST_DATABASE_URL")
	if dbURL == "" {
		dbURL = "postgres://postgres:postgres@localhost:5432/syncmind_test?sslmode=disable"
	}

	db, err := sql.Open("pgx", dbURL)
	if err != nil {
		t.Skipf("Skipping migration test: failed to connect to database: %v", err)
	}
	defer db.Close()

	if err := db.Ping(); err != nil {
		t.Skipf("Skipping migration test: database unreachable: %v", err)
	}

	// Clean slate: drop tables if they exist from previous runs
	_, _ = db.Exec("DROP TABLE IF EXISTS sync_bundles CASCADE")
	_, _ = db.Exec("DROP TABLE IF EXISTS pairing_sessions CASCADE")
	_, _ = db.Exec("DROP TABLE IF EXISTS devices CASCADE")

	gooseBin, err := exec.LookPath("goose")
	if err != nil {
		gooseBin = "go"
	}

	args := []string{"-dir", "../../migrations", "postgres", dbURL, "up"}
	if gooseBin == "go" {
		args = append([]string{"run", "github.com/pressly/goose/v3/cmd/goose@latest", "-dir", "../../migrations", "postgres", dbURL, "up"}, args[5:]...)
	}

	cmd := exec.Command(gooseBin, args...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("goose up failed: %v\n%s", err, out)
	}

	tables := []string{"devices", "pairing_sessions", "sync_bundles"}
	for _, table := range tables {
		var count int
		query := fmt.Sprintf("SELECT COUNT(*) FROM information_schema.tables WHERE table_name = '%s'", table)
		if err := db.QueryRow(query).Scan(&count); err != nil {
			t.Fatalf("failed to check table %s: %v", table, err)
		}
		if count == 0 {
			t.Fatalf("expected table %s to exist after migrations", table)
		}
	}

	// Verify indexes exist
	var idxCount int
	if err := db.QueryRow("SELECT COUNT(*) FROM pg_indexes WHERE indexname = 'idx_sync_bundles_to_device_acked'").Scan(&idxCount); err != nil {
		t.Fatalf("failed to check index: %v", err)
	}
	if idxCount == 0 {
		t.Fatal("expected index idx_sync_bundles_to_device_acked to exist")
	}

	// Rollback
	argsDown := []string{"-dir", "../../migrations", "postgres", dbURL, "down"}
	if gooseBin == "go" {
		argsDown = append([]string{"run", "github.com/pressly/goose/v3/cmd/goose@latest", "-dir", "../../migrations", "postgres", dbURL, "down"}, argsDown[5:]...)
	}

	cmdDown := exec.Command(gooseBin, argsDown...)
	outDown, err := cmdDown.CombinedOutput()
	if err != nil {
		t.Fatalf("goose down failed: %v\n%s", err, outDown)
	}

	for _, table := range tables {
		var count int
		query := fmt.Sprintf("SELECT COUNT(*) FROM information_schema.tables WHERE table_name = '%s'", table)
		if err := db.QueryRow(query).Scan(&count); err != nil {
			t.Fatalf("failed to check table %s after rollback: %v", table, err)
		}
		if count != 0 {
			t.Fatalf("expected table %s to be dropped after rollback", table)
		}
	}
}
